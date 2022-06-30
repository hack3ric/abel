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

function safe_getmetatable(t)
  local type_t = type(t)
  assert(
    type_t == "table",
    "bad argument #1 to 'getmetatable' (table expected, got " .. type_t .. ")"
  )
  return getmetatable(t)
end
