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
    if type_detail ~= "nil" and type_detail ~= "string" and type_detail ~= "table" then
      lua_error "error detail must be nil, string or table"
    end
  end
  lua_error(msg, level)
end

function safe_getmetatable(t)
  local typet = type(t)
  if typet == "table" then
    getmetatable(t)
  else
    error("bad argument #1 to 'getmetatable' (table expected, got" .. typet .. ")")
  end
end
