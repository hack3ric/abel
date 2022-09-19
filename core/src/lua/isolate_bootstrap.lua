local source, remote, create_require = ...

local local_env = {}
local internal = {
  paths = {},
  sealed = false,
}

local require, package = create_require(source, remote, local_env)
local_env.require = require
internal.package = package

return local_env, internal
