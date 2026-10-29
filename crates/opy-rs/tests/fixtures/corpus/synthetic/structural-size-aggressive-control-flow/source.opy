globalvar g

rule "default comparison":
    @Event global
    if g == 1:
        g = 2

#!optimizeForSizeAggressive

rule "aggressive alone comparison":
    @Event global
    if g == 1:
        g = 2

#!optimizeForSize

rule "tail comparison":
    @Event global
    if g == 1:
        g = 2

rule "tail negation":
    @Event global
    if not g:
        g = 2
