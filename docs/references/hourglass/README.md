# Hourglass model references

This directory holds durable source material for Medinote's hourglass simulation. The
[as-built model reference](model.md) describes the production algorithm, product extensions,
tuning boundaries, host workflow, and measured target budget. The completed replacement history is
retained in the [archived paper-model plan](../../archive/features/hourglass-paper-model.md).

## Model anchors and product objective

[Devlin and Schuster](papers/devlin-schuster-2020-probabilistic-cellular-automata.md) is the canonical
source for the base 2x2 transition function. It is the only paper in this set that specifies a small,
real-time cellular automaton for an hourglass, including the block schedule, transition set, and
tested probability range.

Medinote is not trying to reproduce one paper. The product objective is the most natural-looking,
deterministic result that conserves sand and fits the device's memory and 30 Hz physics budget. The
paper transition table remains isolated and directly tested; glass geometry, initial packing,
15-minute pacing, arbitrary-angle scheduling, and the bounded free-surface repose pass are explicit
product extensions.

The Spot Model papers explain why free-surface relaxation and dense drainage may need treatment
beyond a small block CA. They are physical motivation and comparison material. Medinote's local
repose stencil is not an implementation of the Spot Model, whose additional state and computation
would be disproportionate here.

## Local papers

| Role | Paper | Local files | Why it is relevant |
| --- | --- | --- | --- |
| Base rule | Jonathan Devlin and Micah D. Schuster, “Probabilistic Cellular Automata for Granular Media in Video Games” (2020/2021), [arXiv](https://arxiv.org/abs/2008.06341), [DOI](https://doi.org/10.1007/s40869-020-00122-4) | [PDF](papers/devlin-schuster-2020-probabilistic-cellular-automata.pdf), [text extraction](papers/devlin-schuster-2020-probabilistic-cellular-automata.md) | Direct hourglass model: modified four-phase Margolus neighborhood and probabilistic toppling. |
| Physical motivation | Martin Z. Bazant, “The Spot Model for random-packing dynamics” (2005/2006), [arXiv](https://arxiv.org/abs/cond-mat/0501130), [DOI](https://doi.org/10.1016/j.mechmat.2005.06.016) | [PDF](papers/bazant-2005-spot-model.pdf), [text extraction](papers/bazant-2005-spot-model.md) | A collective free-volume model that explains why dense drainage is not only independent grain motion. |
| Physical motivation | Chris H. Rycroft, Yee Lok Wong, and Martin Z. Bazant, “Fast spot-based multiscale simulations of granular drainage” (2010), [DOI](https://doi.org/10.1016/j.powtec.2010.01.009) | [PDF](papers/rycroft-wong-bazant-2010-fast-spot-drainage.pdf), [text extraction](papers/rycroft-wong-bazant-2010-fast-spot-drainage.md) | Separates free-surface handling and shows the complexity of a full Spot Model implementation. |

## Relevant sources not mirrored

These remain part of the literature trail, but no unrestricted author or repository PDF was found
during this pass. Keep links to the publisher record rather than committing a copy with unclear
redistribution status.

| Paper | Relevance |
| --- | --- |
| G. William Baxter and R. P. Behringer, “Cellular automata models of granular flow” (1990), [DOI](https://doi.org/10.1103/PhysRevA.42.1017) | Early experiment-derived CA for nonspherical grains; cited background for free-surface and channel behavior. |
| Jean-Philippe Bouchaud, Michael E. Cates, J. Ravi Prakash, and Sam F. Edwards, “A model for the dynamics of sandpile surfaces” (1994), [DOI](https://doi.org/10.1051/jp1:1994195) | Separates static and rolling populations and explains slope-dependent exchange, but is a continuum surface model rather than the selected CA. |
| Alexander V. Potapov and Charles S. Campbell, “Computer simulation of hopper flow” (1996), [DOI](https://doi.org/10.1063/1.869069) | Supports settling a randomized fill behind a closed outlet before beginning a drainage experiment. |
| Dominique Désérable, “A Versatile Two-dimensional Cellular Automata Network for Granular Flow” (2002), [DOI](https://doi.org/10.1137/S0036139999355205) | A richer synchronous request-exchange lattice-grain model; useful only if the simpler canonical CA fails. |

## Extraction policy

The Markdown files are generated with Poppler's `pdftotext -nopgbrk -enc UTF-8`. They deliberately
retain wording and extraction errors instead of becoming hand-edited summaries. Use them for search
and machine reading; consult the neighboring PDF whenever a transition diagram, equation, table, or
page layout matters.

When refreshing a paper, download from the source named in its extraction header, regenerate the
Markdown, update the recorded SHA-256, and visually compare representative rendered pages before
committing the result.
