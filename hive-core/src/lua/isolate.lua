-- Fields with `nil` should be initialized in Rust

-- Internal --

local paths = {}

local package = {
  loaded = {},
  preload = {},
  searchers = nil,
}

local internal = {
  paths = paths,
  sealed = false,
  source = nil,
  package = package,
}

-- Hive table --

local function register(path, handler)
  assert(
    not internal.sealed,
    "cannot call `hive.register` from places other than the top level of `main.lua`"
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

local hive = {
  register = register,
  context = nil,
  current_worker = current_worker,
  Error = hive_Error,
}

-- Local env --

local function require(modname)
  local modname_type = type(modname)
  assert(
    modname_type == "string",
    "bad argument #1 to 'require' (string expected, got " .. modname_type .. ")"
  )

  local error_msgs = {}
  if package.loaded[modname] then
    return table.unpack(package.loaded[modname])
  else
    for _, searcher in ipairs(package.searchers) do
      local loader, data = searcher(modname)
      if loader then
        local result = { loader(modname, data) }
        package.loaded[modname] = result
        return table.unpack(result)
      else
        table.insert(error_msgs, data)
      end
    end
  end
  error("module '" .. modname .. "' not found:\n\t" .. table.concat(error_msgs, "\n"))
end

local local_env = {
  hive = hive,
  require = require,
}

-- Searchers --

local function preload_searcher(modname)
  local loader = package.preload[modname]
  if loader then
    return loader, "<preload>"
  else
    return nil, "preload '" .. modname .. "' not found"
  end
end

local function source_searcher(modname)
  local source = internal.source
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

-- Standard library whitelist --

local whitelist = {
  [false] = {
    "assert", "ipairs", "next", "pairs",
    "pcall", "print", "rawequal", "select",
    "setmetatable", "tonumber", "tostring", "type",
    "warn", "xpcall", "_VERSION",
  },
  math = {
    "abs", "acos", "asin", "atan",
    "atan2", "ceil", "cos", "deg",
    "exp", "floor", "fmod", "frexp",
    "huge", "ldexp", "log", "log10",
    "max", "maxinteger", "min", "mininteger",
    "modf", "pi", "pow", "rad", "random",
    "sin", "sinh", "sqrt", "tan",
    "tanh", "tointeger", "type", "ult",
  },
  os = {
    "clock", "difftime", "time",
  },
  string = {
    "byte", "char", "find", "format",
    "gmatch", "gsub", "len", "lower",
    "match", "reverse", "sub", "upper",
  },
  table = {
    "remove", "sort", "concat", "pack",
    "unpack",
  },
  coroutine = {
    "close", "create", "isyieldable", "resume",
    "running", "status", "wrap", "yield",
  },
}

local monkey_patch = {
  [false] = {
    "error",
  },
  table = {
    "insert", "dump", "scope"
  },
  routing = "*"
}

local function apply_whitelist(whitelist)
  for module, fields in pairs(whitelist) do
    local from_module, to_module
    if module then
      from_module = _G[module]
      to_module = {}
      local_env[module] = to_module
    else
      from_module = _G
      to_module = local_env
    end

    if fields == "*" then
      for k, v in pairs(from_module) do
        to_module[k] = v
      end
    else
      for _, field in ipairs(fields) do
        to_module[field] = from_module[field]
      end
    end
  end
end

apply_whitelist(whitelist)
apply_whitelist(monkey_patch)

local_env.getmetatable = safe_getmetatable

return local_env, internal
