-- Extract pages: a new PDF with only the pages asked for, one for each PDF chosen.
-- qpdf <in> --pages . <pages> -- <out>

-- qpdf's ranges ("1-3,7", "z" is the last page, "r2" the one before it) and nothing else:
-- no text can reach qpdf as one of its options.
local ONE = "(?:[0-9]+|z|r[0-9]+)"
local RANGE = ONE .. "(?:-" .. ONE .. ")?"
local PAGES = "^" .. RANGE .. "(?:," .. RANGE .. ")*(?::(?:even|odd))?$"

local function size(path)
  local s = rada.stat(path)
  return s and s.size or nil
end

rada.recipe {
  name = "extract-pages",
  title = { en = "Extract pages", it = "Estrai pagine" },
  description = {
    en = "A new PDF with only these pages, such as 1-3,7 (qpdf)",
    it = "Un PDF nuovo con solo queste pagine, per esempio 1-3,7 (qpdf)",
  },
  when = { ext = "pdf" },
  args = {
    { name = "pages", type = "text", label = { en = "Pages (1-3,7; z is the last)", it = "Pagine (1-3,7; z è l'ultima)" } },
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
        args = { rada.input(1), "--pages", ".", pages, "--", rada.out },
        inputs = input,
        output = rada.join(rada.parent(input), rada.stem(input) .. "-pages.pdf"),
        size_hint = size(input),
      }
    end
  end,
}
