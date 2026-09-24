globalvar g

rule "terminal conditionals":
    @Event eachPlayer
    @Condition g > 0
    if g == 1:
        return
    if ruleCondition:
        loop()
    if not ruleCondition:
        return
    if true:
        return
    if false:
        return
    g = 2
    if g == 3:
        loop()
    return
