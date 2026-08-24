globalvar g
rule "r":
    @Event global
    @Condition len(range(1, 5, 1)) > 0
    disableInspector()
