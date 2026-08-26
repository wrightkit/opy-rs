globalvar g

rule "r":
    @Event global
    for I in range(start=0, stop=3):
        debug(I)
