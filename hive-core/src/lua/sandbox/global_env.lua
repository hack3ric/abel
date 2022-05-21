function hive_Error(obj)
  local status = obj.status
  local error = obj.error

  local result = {
    status = status,
    error = error,
  }
  local result_mt = {
    __call = function(detail)
      return {
        status = status,
        error = error,
        detail = detail,
      }
    end
  }

  return setmetatable(result, result_mt)
end

local lua_error = error

function error(msg, level)
  if type(msg) == "table" then
    local type_detail = type(msg.detail)
    assert(
      type_detail == "nil" or type_detail == "string" or type_detail == "table",
      "error detail must be nil, string or table"
    )
  end
  lua_error(msg, level)
end

function safe_getmetatable(t)
  local type_t = type(t)
  assert(
    type_t == "table",
    "bad argument #1 to 'getmetatable' (table expected, got" .. type_t .. ")"
  )
  getmetatable(t)
end
