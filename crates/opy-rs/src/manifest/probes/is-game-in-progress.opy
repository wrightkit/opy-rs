globalvar g
rule "r":
    @Event global
    @Condition isGameInProgress() == true
    disableInspector()
