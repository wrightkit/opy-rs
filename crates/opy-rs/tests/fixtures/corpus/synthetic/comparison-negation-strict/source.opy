#!optimizeStrict
globalvar g
globalvar h

rule "value equal":
    @Event eachPlayer
    h = not (g == 2)

rule "value unequal":
    @Event eachPlayer
    h = not (g != 2)

rule "value less":
    @Event eachPlayer
    h = not (g < 2)

rule "value less or equal":
    @Event eachPlayer
    h = not (g <= 2)

rule "value greater":
    @Event eachPlayer
    h = not (g > 2)

rule "value greater or equal":
    @Event eachPlayer
    h = not (g >= 2)

rule "value double negation":
    @Event eachPlayer
    h = not not (g == 2)

rule "value membership":
    @Event eachPlayer
    h = not (g in [1, 2])

rule "value negated membership":
    @Event eachPlayer
    h = not (g not in [1, 2])

rule "value constant comparison":
    @Event eachPlayer
    h = not (1 == 2)

rule "value identical operands":
    @Event eachPlayer
    h = not (g == g)

rule "value boolean comparison":
    @Event eachPlayer
    h = not ((g == 2) == false)

rule "value alive":
    @Event eachPlayer
    h = not eventPlayer.isAlive()

rule "if equal":
    @Event eachPlayer
    if not (g == 2):
        h = 1

rule "condition equal":
    @Event eachPlayer
    @Condition not (g == 2)
    h = 1

rule "if unequal":
    @Event eachPlayer
    if not (g != 2):
        h = 1

rule "condition unequal":
    @Event eachPlayer
    @Condition not (g != 2)
    h = 1

rule "if less":
    @Event eachPlayer
    if not (g < 2):
        h = 1

rule "condition less":
    @Event eachPlayer
    @Condition not (g < 2)
    h = 1

rule "if less or equal":
    @Event eachPlayer
    if not (g <= 2):
        h = 1

rule "condition less or equal":
    @Event eachPlayer
    @Condition not (g <= 2)
    h = 1

rule "if greater":
    @Event eachPlayer
    if not (g > 2):
        h = 1

rule "condition greater":
    @Event eachPlayer
    @Condition not (g > 2)
    h = 1

rule "if greater or equal":
    @Event eachPlayer
    if not (g >= 2):
        h = 1

rule "condition greater or equal":
    @Event eachPlayer
    @Condition not (g >= 2)
    h = 1

rule "if double negation":
    @Event eachPlayer
    if not not (g == 2):
        h = 1

rule "condition double negation":
    @Event eachPlayer
    @Condition not not (g == 2)
    h = 1
