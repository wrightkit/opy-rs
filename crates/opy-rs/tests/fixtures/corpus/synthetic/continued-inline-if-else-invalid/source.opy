globalvar value

macro nextHero():
    value = 1\
    if value == 0: value = 2\
    else: value = 3\

rule "probe":
    @Event global
    nextHero()
