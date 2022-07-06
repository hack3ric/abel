local http = require "http"

abel.register("/", function(req)
  local resp = http.request "https://httpbin.org/get"

  return {
    resp_status = resp.status,
    result = resp.body:parse_json(),
  }
end)
