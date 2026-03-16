[tools]
Available scripts (assume installed):

```bash
# Read / extract text
python -m markitdown file.pptx          # Extract text from Office files
pandoc --track-changes=all file.docx -o out.md  # Docx with tracked changes

# Visual preview
python scripts/thumbnail.py file.pptx   # Generate slide thumbnails

# Unpack / repack XML
python scripts/office/unpack.py file.docx unpacked/
python scripts/office/repack.py unpacked/ file.docx

# LibreOffice conversion
python scripts/office/soffice.py --headless --convert-to pdf file.docx
python scripts/office/soffice.py --headless --convert-to docx file.doc

# Formula recalculation (xlsx)
python scripts/recalc.py file.xlsx

# Validation
python scripts/office/validate.py file.docx
```

[workflow]
Standard edit workflow for existing Office files:
1. Read/analyze: use markitdown or pandoc to understand current content
2. Unpack: `unpack.py` to access raw XML
3. Edit: modify XML directly for structural changes
4. Repack: `repack.py` to rebuild the file
5. Validate: `validate.py` to confirm no corruption
6. If formulas involved (xlsx): run `recalc.py` after saving

For legacy `.doc` files, convert first:
```bash
python scripts/office/soffice.py --headless --convert-to docx input.doc
```
