-- Merge PDFs: one new PDF with the pages of the files chosen, in the order they were chosen.
-- qpdf --empty --pages <inputs> -- <out>

local function size(path)
  local s = rada.stat(path)
  return s and s.size or 0
end

rada.recipe {
  name = "merge-pdf",
  title = { en = "Merge PDFs", it = "Unisci PDF" },
  description = {
    en = "One new PDF with the pages of all the files chosen (qpdf)",
    it = "Un PDF nuovo con le pagine di tutti i file scelti (qpdf)",
  },
  when = { ext = "pdf", min = 2 },
  available = function()
    return rada.has("qpdf")
  end,
  run = function(ctx)
    local first = ctx.selection[1]
    local total = 0
    for _, input in ipairs(ctx.selection) do
      total = total + size(input)
    end
    rada.run {
      program = "qpdf",
      args = { "--empty", "--pages", rada.inputs, "--", rada.out },
      inputs = ctx.selection,
      output = rada.join(rada.parent(first), rada.stem(first) .. "-merged.pdf"),
      size_hint = total > 0 and total or nil,
    }
  end,
}
