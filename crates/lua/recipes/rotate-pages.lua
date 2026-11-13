-- Rotate pages: a new PDF with the pages turned, one for each PDF chosen.
-- qpdf <in> --rotate=+<angle>:<pages> <out>

local ONE = "(?:[0-9]+|z|r[0-9]+)"
local RANGE = ONE .. "(?:-" .. ONE .. ")?"
local PAGES = "^" .. RANGE .. "(?:," .. RANGE .. ")*(?::(?:even|odd))?$"

local function size(path)
  local s = rada.stat(path)
  return s and s.size or nil
end

rada.recipe {
  name = "rotate-pages",
  title = { en = "Rotate pages", it = "Ruota pagine" },
  description = {
    en = "A new PDF with the pages turned clockwise (qpdf)",
    it = "Un PDF nuovo con le pagine girate in senso orario (qpdf)",
  },
  when = { ext = "pdf" },
  args = {
    { name = "angle", label = { en = "Angle", it = "Angolo" }, choices = { "90", "180", "270" }, default = "90" },
    { name = "pages", type = "text", label = { en = "Pages (1-z is all)", it = "Pagine (1-z è tutte)" }, default = "1-z" },
  },
  available = function()
    return rada.has("qpdf")
  end,
  run = function(ctx, args)
    local pages = rada.replace(args.pages, "\\s+", "")
    if rada.match(pages, PAGES) == nil then
      if ctx.lang == "it" then
        error("pagine non valide: " .. args.pages .. " (per esempio 1-3,7)", 0)
      end
      error("these are not pages: " .. args.pages .. " (for example 1-3,7)", 0)
    end
    for _, input in ipairs(ctx.selection) do
      rada.run {
        program = "qpdf",
        args = { rada.input(1), "--rotate=+" .. args.angle .. ":" .. pages, rada.out },
        inputs = input,
        output = rada.join(rada.parent(input), rada.stem(input) .. "-rotated.pdf"),
        size_hint = size(input),
      }
    end
  end,
}
