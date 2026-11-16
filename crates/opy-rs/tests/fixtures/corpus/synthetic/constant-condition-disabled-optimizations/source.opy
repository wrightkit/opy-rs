#!disableOptimizations
globalvar g

rule "true literal":
    @Event global
    @Condition true
    g = 1

rule "false literal":
    @Event global
    @Condition false
    g = 2

rule "folded true":
    @Event global
    @Condition 1 + 2 == 3
    g = 3
