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

rule "tail negated equality":
    @Event global
    if not (g == 1):
        g = 2

rule "tail negated inequality":
    @Event global
    if not (g != 1):
        g = 2

rule "tail negated less than":
    @Event global
    if not (g < 1):
        g = 2

rule "tail negated less than or equal":
    @Event global
    if not (g <= 1):
        g = 2

rule "tail negated greater than":
    @Event global
    if not (g > 1):
        g = 2

rule "tail negated greater than or equal":
    @Event global
    if not (g >= 1):
        g = 2

rule "tail double negated equality":
    @Event global
    if not not (g == 1):
        g = 2

rule "tail double negated inequality":
    @Event global
    if not not (g != 1):
        g = 2

rule "tail double negated less than":
    @Event global
    if not not (g < 1):
        g = 2

rule "tail double negated less than or equal":
    @Event global
    if not not (g <= 1):
        g = 2

rule "tail double negated greater than":
    @Event global
    if not not (g > 1):
        g = 2

rule "tail double negated greater than or equal":
    @Event global
    if not not (g >= 1):
        g = 2
