local http = require "http"

hive.register("/", function(req)
  local resp = http.request "https://httpbin.org/get"

  return {
    status = resp.status,
    result = resp.body:parse_json(),
  }
end)
