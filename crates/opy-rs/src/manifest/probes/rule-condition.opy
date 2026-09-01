rule "rule condition":
    @Event global
    @Condition isGameInProgress()
    @Condition isAssemblingHeroes() == false
    @Condition isGameInProgress()
    waitUntil(RULE_CONDITION, 1)
