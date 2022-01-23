local context = require "context"

function hive.start()
  context:set("count", 0)
end

hive.register("/", function(req)
  local _, count = context:set("count", function(count) return count + 1 end)
  return {
    count = count,
    test = context:get "test array",
    method = req.method
  }
end)

hive.register("a/:name/as/fdf/", function(req)
  return { greeting = "Hello, \128" .. req.params.name .. "!" }
end)
