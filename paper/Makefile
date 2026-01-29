LATEXMK ?= latexmk

all: paper.pdf

paper.pdf: paper.tex references.bib figures/pipeline.tikz
	$(LATEXMK) -pdf -interaction=nonstopmode -halt-on-error paper.tex

clean:
	$(LATEXMK) -C
	$(RM) paper.pdf

.PHONY: all clean
