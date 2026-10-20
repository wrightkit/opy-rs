#!optimizeForSize

globalvar g
globalvar h

rule "operands":
    @Event global
    @Condition g >= 1
    @Condition g < 1
    @Condition 1 <= g
    @Condition g > 1
    @Condition g == 1
    @Condition g != 1
    @Condition g >= 0
    @Condition g == 0
    @Condition (g + 1) >= 1
    @Condition h
    h = 1
