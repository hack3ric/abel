local source, remote, create_require, sha256 = ...

local nop_global_table = {}
local nop_table = {}

local function instant_return(value)
  return function() return value end
end

local return_nop = instant_return(nop_table)

local nop_metatable = {
  __index = return_nop,
  __newindex = instant_return(nil),

  __add = return_nop,
  __sub = return_nop,
  __mul = return_nop,
  __div = return_nop,
  __mod = return_nop,
  __pow = return_nop,
  __unm = return_nop,
  __idiv = return_nop,
  __band = return_nop,
  __bor = return_nop,
  __bxor = return_nop,
  __bnot = return_nop,
  __shl = return_nop,
  __shr = return_nop,
  __concat = return_nop,
  __len = instant_return(0),
  __eq = instant_return(false),
  __lt = instant_return(false),
  __le = instant_return(false),
  __call = function()
    return nop_table, nop_table, nop_table, nop_table, nop_table, nop_table, nop_table, nop_table, nop_table, nop_table
  end,
}

setmetatable(nop_table, nop_metatable)
setmetatable(nop_global_table, nop_metatable)

local hashes = {}
local remote_wrapper = {
  load = function(_, modname, uri, env)
    local code, req_uri = remote:get(modname, uri)
    local r = #modname > 0 and modname .. " @" .. uri or "@" .. uri
    hashes[r] = sha256(code)
    load(code, "@" .. tostring(req_uri), "t", env)()
    return nop_table
  end
}

local require, package = create_require(source, remote_wrapper, nop_global_table)

local function require_wrapper(modname)
  if type(modname) == "string" then
    return require(modname)
  end
end

rawset(nop_global_table, "require", require_wrapper)

local stdlibs = {
  "math", "string", "table", "coroutine",
  "os", "utf8", "fs", "http",
  "json", "rand", "crypto", "stream",
  "testing",
}
for _, v in ipairs(stdlibs) do
  package.preload[v] = return_nop
end

source:load("main.lua", nop_global_table)()

return hashes
