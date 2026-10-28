#!optimizeForSize
#!replace0ByIsMatchComplete
#!replace1ByMatchRound
#!replaceTeam1ByControlScoringTeam
#!replaceEmptyStringByEmptyArray

globalvar code
globalvar g
globalvar n

rule "operands":
    @Event global
    @Condition code == ""
    @Condition code != ""
    @Condition g == 0
    @Condition g == 1
    @Condition g == Team.1
    @Condition g <= ""
    n = 2

rule "reuse":
    @Event global
    @Condition code == ""
    while RULE_CONDITION:
        n = 2
        wait(0.5)
