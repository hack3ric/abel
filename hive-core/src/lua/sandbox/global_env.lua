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
