# Build and package arXiv sources

Run these commands from the repository root.

## 1) Compile

If there is **no bibliography** (`paper/main.bib` does not exist):

```bash
pdflatex -interaction=nonstopmode -halt-on-error -output-directory=paper paper/main.tex
```

If there **is** a bibliography (`paper/main.bib` exists):

```bash
pdflatex -interaction=nonstopmode -halt-on-error -output-directory=paper paper/main.tex
bibtex paper/main
pdflatex -interaction=nonstopmode -halt-on-error -output-directory=paper paper/main.tex
pdflatex -interaction=nonstopmode -halt-on-error -output-directory=paper paper/main.tex
```

## 2) Create minimal arXiv zip

If there is **no bibliography**:

```bash
(cd paper && zip -r ../paper_arxiv.zip main.tex *.sty *.cls figures/)
```

If there **is** a bibliography (include generated `main.bbl`):

```bash
(cd paper && zip -r ../paper_arxiv.zip main.tex main.bbl *.bib *.sty *.cls figures/)
```

Adjust optional globs (`*.sty`, `*.cls`, `figures/`) to match files actually used by `main.tex`.
