local json = require "json"

function hive.start()
  names = json.array {}
end

local function hello(req)
  local name = req.params.name or "world"
  table.insert(names, name)
  return { greeting = "Hello, " .. name .. "!" }
end

local function list(req)
  return hive.shared.names
end

hive.register("/", hello)
hive.register("/list", list)
hive.register("/:name", hello)
