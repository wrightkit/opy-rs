# Upstream OverPy rejects `None`; `null` is the only null literal.
rule "x":
    @Event eachPlayer
    if A == None:
        pass
