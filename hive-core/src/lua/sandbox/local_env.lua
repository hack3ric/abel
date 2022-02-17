local internal = {
  paths = {},
  sealed = false,
  package = {
    loaded = {},
    preload = {},
    searchers = nil,
  },
}

-- Fields with `nil` should be initialized in Rust
local local_env = {
  hive = {
    register = function(path, handler)
      if internal.sealed then
        error "cannot call `hive.register` from places other than the top level of `main.lua`"
      end
      table.insert(internal.paths, { path, handler })
    end,
    context = nil,
    create_response = _G.create_response,
    current_worker = _G.current_worker,
  },

  require = function(modname)
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
  end,
}

local function source_searcher(modname)
  local source = internal.source
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

local function lib_searcher()
  -- TODO
end

internal.package.searchers = { source_searcher, lib_searcher }

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
    "dump",
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
