local QuartoLadybugVersion = "0.1.0"
local hasDoneLadybugSetup = false
local counter = 0

-- Default ladybug-rs endpoint
local LADYBUG_ENDPOINT = os.getenv("LADYBUG_ENDPOINT") or "http://127.0.0.1:8080"

local function ensureLadybugSetup()
  if hasDoneLadybugSetup then
    return
  end

  hasDoneLadybugSetup = true

  quarto.doc.add_html_dependency({
    name = "ladybug-query",
    version = QuartoLadybugVersion,
    stylesheets = {
      "ladybug-query.css",
    },
    scripts = {
      { path = "ladybug-query.js", afterBody = true }
    },
  })
end


-- Initialize default cell-level options
local defaultCellOptions = {
  ["context"] = "interactive",
  ["endpoint"] = LADYBUG_ENDPOINT,
}

-- Shallow copy helper
local function shallowcopy(original)
  if type(original) == 'table' then
    local copy = {}
    for key, value in pairs(original) do
        copy[key] = value
    end
    return copy
  else
    return original
  end
end

-- Merge local cell options with defaults
local function mergeCellOptions(localOptions)
  local mergedOptions = shallowcopy(defaultCellOptions)
  for key, value in pairs(localOptions) do
    mergedOptions[key] = value
  end
  return mergedOptions
end


-- Extract Quarto code cell options from the block's text
-- Cypher comment syntax: // | key: value
local function extractCodeBlockOptions(block)
  local code = block.text
  local cellOptions = {}
  local newCodeLines = {}

  for line in code:gmatch("([^\r\n]*)[\r\n]?") do
    -- Check for //| key: value pattern (Cypher-friendly comment)
    local key, value = line:match("^//|%s*(.-):%s*(.-)%s*$")

    if key and value then
      cellOptions[key] = value
    else
      table.insert(newCodeLines, line)
    end
  end

  cellOptions = mergeCellOptions(cellOptions)
  local cellCode = table.concat(newCodeLines, '\n')
  return cellCode, cellOptions
end


-- Remove leading empty lines
local function removeEmptyLinesUntilContent(codeText)
  local lines = {}
  for line in codeText:gmatch("([^\r\n]*)[\r\n]?") do
    table.insert(lines, line)
  end

  while #lines > 0 and lines[1]:match("^%s*$") do
    table.remove(lines, 1)
  end

  return table.concat(lines, '\n')
end


function CodeBlock(el)

  ensureLadybugSetup()

  local no_attrs = not el.attr
  local not_html = not quarto.doc.is_format("html")
  local not_ladybug = not el.attr.classes:includes("{ladybug}")

  if no_attrs or not_html or not_ladybug then
    return el
  end

  counter = counter + 1

  local cellCode, cellOpts = extractCodeBlockOptions(el)

  el["text"] = removeEmptyLinesUntilContent(cellCode)

  -- Store endpoint as data attribute for JS to pick up
  local endpoint = cellOpts["endpoint"] or LADYBUG_ENDPOINT

  el["attr"]["classes"] = {
    "cypher",
    "cell-code",
    "ladybug-query",
    "language-cypher",
    "code-with-copy",
  }

  -- Tag with block ID and endpoint for JS
  el["attr"]["attributes"]["data-ladybug-id"] = tostring(counter)
  el["attr"]["attributes"]["data-ladybug-endpoint"] = endpoint

  return el

end
