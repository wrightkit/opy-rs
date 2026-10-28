#!optimizeForSize
#!replaceEmptyStringByVariable

globalvar code
globalvar n

rule "operands":
    @Event global
    @Condition code == ""
    @Condition code != ""
    while RULE_CONDITION:
        n = 1
        wait(1)
