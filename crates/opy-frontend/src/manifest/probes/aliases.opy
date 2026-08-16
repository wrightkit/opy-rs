globalvar g
rule "r":
    @Event global
    stopChasingVariable(g)
    stopChasing(g)
