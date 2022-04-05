local json = require "json"

local function hello(req)
  local name = req.params.name or "world"
  return { greeting = "Hello, " .. name .. "(new) !" }
end

local function list(req)
  return json.array(table.dump(hive.context))
end

hive.register("/", hello)
hive.register("/list", list)
hive.register("/:name", hello)
