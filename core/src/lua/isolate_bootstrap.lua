-- Fields with `nil` should be initialized in Rust

local paths, loaded, preload = {}, {}, {}

local package = {
  loaded = loaded,
  preload = preload,
}

local internal = {
  paths = paths,
  sealed = false,
  source = nil,
  package = package,
}

local abel = {
  current_worker = current_worker,
  Error = abel_Error,
}

local local_env = {
  abel = abel
}

function abel.register(path, handler)
  assert(
    not internal.sealed,
    "cannot call `abel.register` from places other than the top level of `main.lua`"
  )
  local type_handler = type(handler)
  if type_handler ~= "function" then
    if type_handler == "table" then
      local mt = getmetatable(handler)
      if type(mt) == "table" and type(mt.__call) == "function" then
        goto ok
      end
    end
    error "handler must either be a function or a callable table"
  end

  ::ok::
  table.insert(paths, { path, handler })
end

function local_env.require(modname)
  local modname_type = type(modname)
  assert(
    modname_type == "string",
    "bad argument #1 to 'require' (string expected, got " .. modname_type .. ")"
  )

  local error_msgs = {}
  if loaded[modname] then
    return table.unpack(loaded[modname])
  else
    for _, searcher in ipairs(package.searchers) do
      local loader, data = searcher(modname)
      if loader then
        local result = { loader(modname, data) }
        loaded[modname] = result
        return table.unpack(result)
      else
        table.insert(error_msgs, data)
      end
    end
  end
  error("module '" .. modname .. "' not found:\n\t" .. table.concat(error_msgs, "\n\t"))
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

local function source_searcher(modname)
  local source = internal.source
  if not source then
    return nil, "source not enabled in this isolate"
  end

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

package.searchers = { preload_searcher, source_searcher }

return local_env, internal
