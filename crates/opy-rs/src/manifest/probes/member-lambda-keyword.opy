globalvar g

rule "r":
    @Event global
    g = [1, 2].filter(condition=lambda x: x > 0)
