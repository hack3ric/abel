local json = require "json"

function hive.start()
  -- error "panic on start"
  hive.context.something = "foo"
  print "Started"
end

local function hello(req)
  local name = req.params.name or "world"
  table.insert(hive.context, name)
  return { greeting = "Hello, " .. name .. "(new) !" }
end

hive.register("/", hello)

hive.register("/list", function(req)
  for k, v in pairs(hive.context) do
    print(k, v)
  end
  return json.array(table.dump(hive.context))
end)

hive.register("/:name", hello)

hive.register("/:key/:value", function(req)
  hive.context[req.params.key] = req.params.value
end)

function hive.stop()
  -- error "panic on stop"
  print "Stopped"
end
