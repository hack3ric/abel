local source, request, Uri = ...
local local_env, paths, loaded, preload = {}, {}, {}, {}

local package = {
  loaded = loaded,
  preload = preload,
}

local internal = {
  paths = paths,
  sealed = false,
  source = source,
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

local function _require_load_module(modname, searchers)
  if loaded[modname] then
    return table.unpack(loaded[modname])
  else
    local error_msgs = {}
    for _, searcher in ipairs(searchers) do
      local loader, data = searcher(modname)
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
  return _require_load_module(modname, package.searchers)
end

local function _modify_modname(modname, uri)
  if preload[modname] or string.find(modname, "%s*@") then
    return modname
  else
    return modname .. " @" .. uri
  end
end

local function require_remote(uri, modname)
  _require_check_modname_type(modname)
  local modified = _modify_modname(modname, uri)
  return _require_load_module(modified, package.searchers_remote)
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

local function _request_ok(...)
  local resp, err = request(...)
  local status = resp.status
  local content_type = resp.headers["content-type"]

  if err then
    return nil, err
  elseif status ~= 200 then
    return nil, "server responded with status code " .. status
  elseif not content_type then
    return nil, "content type missing"
  elseif not string.find(content_type, "lua", 1, true) then
    return nil, "content type '" .. content_type .. "' does not include 'lua'"
  else
    return resp
  end
end

local remote_local_env_mt = {
  __index = local_env,
  __newindex = local_env,
  __metatable = false
}

-- TODO: add cache
local function remote_searcher(modname)
  local a, z = string.find(modname, "%s*@")
  if a then
    local real_modname = string.sub(modname, 1, a - 1)
    local uri_string = string.sub(modname, z + 1)

    local success, uri = pcall(Uri, uri_string)
    if not success then
      error("invalid uri '" .. uri_string .. "' (" .. uri .. ")")
    end

    local path = ""
    for str in string.gmatch(real_modname, "([^%.]+)") do
      path = path .. "/" .. str
    end

    local uri_params = {
      scheme = uri.scheme,
      authority = uri.authority,
      query = uri.query_string,
    }
    local base_path = uri.path == "/" and path or uri.path .. path
    uri_params.path = base_path .. "/init.lua"
    local init_uri = Uri(uri_params)
    local init_resp, init_err = _request_ok(init_uri)
    uri_params.path = #path == 0 and base_path or base_path .. ".lua"
    local file_uri = Uri(uri_params)
    local file_resp, file_err = _request_ok(file_uri)

    local resp = init_resp or file_resp
    local req_uri = tostring(init_resp and init_uri or file_uri)
    if not resp then
      error(
        "module '" .. modname .. "' not found\n" ..
        "\tfailed to load '" .. tostring(init_uri) .. "' (" .. init_err .. ")\n" ..
        "\tfailed to load '" .. tostring(file_uri) .. "' (" .. file_err .. ")\n"
      )
    elseif file_resp and init_resp then
      error(
        "module '" .. modname .. "' not found\n" ..
        "\tfile '" .. tostring(init_uri) .. "' and '" .. tostring(file_uri) .. "' conflicts"
      )
    end
    local code = resp.body:to_string()

    local remote_local_env = setmetatable({
      require = function(...)
        -- TODO: This passes unparsed URI. Maybe reuse parsed one?
        return require_remote(uri_string, ...)
      end
    }, remote_local_env_mt)

    local loader, err = load(code, "@" .. req_uri, "t", remote_local_env)
    assert(loader, err)

    return loader, req_uri
  else
    return nil, "'" .. modname .. "' does not seem to have a URI"
  end
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
      return source:load(path, local_env)()
    end
    return source_loader, path
  end
end

package.searchers = { preload_searcher, remote_searcher, source_searcher }
package.searchers_remote = { preload_searcher, remote_searcher }

return local_env, internal
