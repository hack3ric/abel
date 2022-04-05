function Error(obj)
  local status = obj.status
  local error = obj.error
  return function(detail)
    local new_detail
    if type(detail) == "table" then
      new_detail = detail
    else
      new_detail = { msg = tostring(detail) }
    end

    return {
      status = status,
      error = error,
      detail = new_detail,
    }
  end
end

function safe_getmetatable(t)
  local typet = type(t)
  if typet == "table" then
    getmetatable(t)
  else
    error("bad argument #1 to 'getmetatable' (table expected, got" .. typet .. ")")
  end
end
