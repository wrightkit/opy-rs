globalvar A
rule "r":
    @Event global
    A = round(-0.5)
    A = round(-2.5)
    A = round(2.5)
    A = round(0.49999999999999994)
    A = 1 if 0.5 > 0 else round(-1.5)
rule "only return":
    @Event global
    return
rule "if return":
    @Event global
    if A == 1:
        return
rule "t":
    @Event global
    if getCurrentMap() == Map.THRONE_OF_ANUBIS:
        A = 1
    if getCurrentMap() == Map.SAMOA:
        A = 2
