local stream = {}
local json_parse = ...

local function check_stream(st)
  local type_st = type(st)
  if type_st ~= "table" and type_st ~= "userdata" or not st.read then
    error("stream expected, got " .. type_st, 3)
  end
end

local function check_sink(sink)
  local type_sink = type(sink)
  if type_sink ~= "table" and type_sink ~= "userdata" or not sink.write then
    error("sink expected, got " .. type_sink, 3)
  end
end

local function check_transform(tr)
  local type_tr = type(tr)
  if type_tr ~= "table" and type_tr ~= "userdata" or not tr.transform then
    error("transform expected, got " .. type_tr, 3)
  end
end

-- Only for items that are concatenatable, usually bytes
function stream.read_all(st)
  check_stream(st)
  local buf = st:read()
  for item in stream.iter(st) do
    buf = buf .. item
  end
  return buf
end

function stream.parse_json(st)
  check_stream(st)
  local str = stream.read_all(st)
  return json_parse(str)
end

function stream.iter(st)
  check_stream(st)
  return function() return st:read() end
end

function stream.from_iter(iter, state, ...)
  local var = { ... }
  return setmetatable({
    read = function(_)
      var = { iter(state, table.unpack(var)) }
      return table.unpack(var)
    end
  }, { __index = stream })
end

function stream.pipe_to(st, ...)
  local sinks = { ... }
  check_stream(st)
  for _, sink in ipairs(sinks) do
    check_sink(sink)
  end
  for item in stream.iter(st) do
    for _, sink in ipairs(sinks) do
      sink:write(item)
    end
  end
end

function stream.pipe_through(st, tr)
  check_stream(st)
  check_transform(tr)
  return setmetatable({
    read = function(_)
      local item = st:read()
      if item then
        return tr:transform(item)
      end
    end
  }, { __index = st })
end

function stream.recv_from(sink, st)
  check_sink(sink)
  check_stream(st)
  stream.pipe_to(st, sink)
end

function stream.recv_through(sink, tr)
  check_sink(sink)
  check_transform(tr)
  return setmetatable({
    write = function(_, item)
      local new_item = tr:transform(item)
      if new_item then
        sink:write(new_item)
      end
    end
  }, { __index = sink })
end

return stream
