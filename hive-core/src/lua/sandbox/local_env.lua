-- Fields with `nil` should be initialized in Rust

-- Internal --

local internal = {
  paths = {},
  sealed = false,
  source = nil,
  permission_bridge = nil,
  package = {
    loaded = {},
    preload = {},
    searchers = nil,
  },
}

-- Hive table --

local function register(path, handler)
  if internal.sealed then
    error "cannot call `hive.register` from places other than the top level of `main.lua`"
  end
  table.insert(internal.paths, { path, handler })
end

local function require(modname)
  local modname_type = type(modname)
  if modname_type ~= "string" then
    error("bad argument #1 to 'require' (string expected, got " .. modname_type .. ")")
  end

  local package = internal.package;
  local error_msgs = {}
  if package.loaded[modname] then
    print "loaded"
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

local function permission_check(perm)
  return internal.permission_bridge:check(perm)
end

-- Local env --

local local_env = {
  hive = {
    register = register,
    context = nil,
    create_response = create_response,
    current_worker = current_worker,
    permission = {
      check = permission_check,
    },
  },
  require = require,
}

-- Searchers --

local preload = internal.package.preload
local function preload_searcher(modname)
  local loader = preload[modname]
  if loader then
    return loader, "<preload>"
  else
    return nil, "preload '" .. modname .. "' not found"
  end
end

local source = internal.source
local function source_searcher(modname)
  local path = ""
  for str in string.gmatch(modname, "([^%.]+)") do
    path = path .. "/" .. str
  end

  local file_exists = source:exists(path .. ".lua")
  local init_exists = source:exists(path .. "/init.lua")

  if file_exists and init_exists then
    return nil, "file '@source:" .. path .. ".lua' and '@source:" .. path .. "/init.lua' conflicts"
  elseif not file_exists and not init_exists then
    return nil, "no file '@source:" .. path .. ".lua'\n\tno file '@source:" .. path .. "/init.lua'"
  else
    path = path .. (file_exists and ".lua" or "/init.lua")
    local function source_loader(modname, path)
      return source:load(path, local_env)()
    end
    return source_loader, path
  end
end

internal.package.searchers = { preload_searcher, source_searcher }

-- Standard library whitelist --

local whitelist = {
  [false] = {
    "assert", "error", "ipairs", "next",
    "pairs", "pcall", "print", "rawequal",
    "select", "setmetatable", "tonumber", "tostring",
    "type", "warn", "xpcall", "_VERSION",
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
    "insert", "maxn", "remove", "sort",
    "dump", "scope",
  },
}

for module, fields in pairs(whitelist) do
  if module then
    local_env[module] = {}
    for _, field in ipairs(fields) do
      local_env[module][field] = _G[module][field]
    end
  else
    for _, field in ipairs(fields) do
      local_env[field] = _G[field]
    end
  end
end

return local_env, internal
