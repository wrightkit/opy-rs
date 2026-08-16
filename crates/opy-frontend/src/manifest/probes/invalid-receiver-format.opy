globalvar g
rule "r":
    @Event global
    print(3.format("{}"))
