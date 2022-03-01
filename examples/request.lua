local request = require "request"

hive.register("/", function(req)
  local resp = request "https://httpbin.org/get"

  return {
    status = resp.status,
    result = resp.body:parse_json(),
  }
end)
