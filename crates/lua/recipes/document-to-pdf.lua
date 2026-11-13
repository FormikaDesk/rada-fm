-- Document to PDF: a PDF of a document, one for each document chosen. LibreOffice names the
-- file by itself, in the work folder; its own profile, also in the work folder, keeps it from
-- waiting for or changing the person's.
-- soffice --headless --convert-to pdf --outdir <OutDir> <ProfileUrl> <in>

local DOCUMENTS = { "doc", "docx", "odt", "rtf", "xls", "xlsx", "ods", "ppt", "pptx", "odp" }

rada.recipe {
  name = "document-to-pdf",
  title = { en = "Document to PDF", it = "Documento in PDF" },
  description = {
    en = "A PDF of the document (LibreOffice)",
    it = "Un PDF del documento (LibreOffice)",
  },
  when = { ext = DOCUMENTS },
  available = function()
    return rada.has("soffice")
  end,
  run = function(ctx)
    for _, input in ipairs(ctx.selection) do
      rada.run {
        program = "soffice",
        args = { "--headless", "--convert-to", "pdf", "--outdir", rada.outdir, rada.profile, rada.input(1) },
        inputs = input,
        output = rada.join(rada.parent(input), rada.stem(input) .. ".pdf"),
        collect = { single = "pdf" },
        timeout = 300,
      }
    end
  end,
}
