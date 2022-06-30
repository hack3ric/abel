local http = require "http"

hive.register("/", function(req)
  -- setmetatable()
  error "Test"
  local resp = http.request "https://httpbin.org/get"

  return {
    status = resp.status,
    result = resp.body:parse_json(),
    h = req.headers["\x00"]
  }
end)
