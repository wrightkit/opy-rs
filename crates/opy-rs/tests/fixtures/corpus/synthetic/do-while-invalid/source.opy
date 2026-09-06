globalvar value

rule "issue 47 invalid do while placement":
    @Event global
    value = 1
    do:
        value += 1
    while value < 2
