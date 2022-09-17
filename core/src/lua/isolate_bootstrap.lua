local source, remote = ...
local local_env, paths, loaded, preload = {}, {}, {}, {}

local package = {
  loaded = loaded,
  preload = preload,
}

local internal = {
  paths = paths,
  sealed = false,
  package = package,
}

-- require --

local function _require_check_modname_type(modname)
  local modname_type = type(modname)
  assert(
    modname_type == "string",
    "bad argument #1 to 'require' (string expected, got " .. modname_type .. ")"
  )
end

local function _require_load_module(searchers, modname, ...)
  if loaded[modname] then
    return table.unpack(loaded[modname])
  else
    local error_msgs = {}
    for _, searcher in ipairs(searchers) do
      local loader, data = searcher(modname, ...)
      if loader then
        local result = { loader(modname, data) }
        loaded[modname] = result
        return table.unpack(result)
      else
        table.insert(error_msgs, data)
      end
    end
    error("module '" .. modname .. "' not found:\n\t" .. table.concat(error_msgs, "\n\t"))
  end
end

function local_env.require(modname)
  _require_check_modname_type(modname)
  return _require_load_module(package.searchers, modname)
end

local function require_remote(uri, modname)
  _require_check_modname_type(modname)
  if not preload[modname] and not string.find(modname, "%s*@") then
    modname = modname .. " @" .. uri
  end
  return _require_load_module(package.searchers_remote, modname)
end

-- Searchers --

local function preload_searcher(modname)
  local loader = preload[modname]
  if loader then
    return loader, "<preload>"
  else
    return nil, "preload '" .. modname .. "' not found"
  end
end

local remote_env_mt = {
  __index = local_env,
  __newindex = local_env,
  __metatable = false
}

local function _check_remote(modname)
  local a, z = string.find(modname, "%s*@")
  if a then
    local path = string.sub(modname, 1, a - 1)
    local uri = string.sub(modname, z + 1)
    return path, uri
  end
end

local function _remote_searcher(path, uri)
  local remote_env = setmetatable({
    require = function(...)
      -- TODO: This passes unparsed URI. Maybe reuse parsed one?
      return require_remote(uri, ...)
    end
  }, remote_env_mt)
  return remote:get(path, uri, remote_env)
end

local function remote_searcher(modname)
  local path, uri = _check_remote(modname)
  if not path then
    return nil, "'" .. modname .. "' does not seem to have a URI"
  end
  return _remote_searcher(path, uri)
end

local function source_searcher(modname)
  local path = ""
  for str in string.gmatch(modname, "([^%.]+)") do
    path = path .. "/" .. str
  end

  local file_exists = source:exists(path .. ".lua")
  local init_exists = source:exists(path .. "/init.lua")

  if file_exists and init_exists then
    return nil, "file 'source:" .. path .. ".lua' and 'source:" .. path .. "/init.lua' conflicts"
  elseif not file_exists and not init_exists then
    return nil, "no file 'source:" .. path .. ".lua'\n\tno file 'source:" .. path .. "/init.lua'"
  else
    path = path .. (file_exists and ".lua" or "/init.lua")
    local function source_loader(modname, path)
      return source:load(path, local_env)(modname, path)
    end

    return source_loader, path
  end
end

package.searchers = { preload_searcher, remote_searcher, source_searcher }
package.searchers_remote = { preload_searcher, remote_searcher }

return local_env, internal
