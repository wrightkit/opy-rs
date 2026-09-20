#!debugElementCount
globalvar value
rule "small":
    @Event global
    @Condition value == 1
    print(1)
rule "large":
    @Event global
    @Condition value == 1
    print(1)
    print(2)
