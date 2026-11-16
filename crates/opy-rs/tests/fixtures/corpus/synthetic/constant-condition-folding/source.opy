globalvar g
globalvar h

rule "true condition":
    @Event global
    @Condition 1 + 2 == 3
    g = 1

rule "literal true":
    @Event global
    @Condition true
    g = 1

rule "false condition":
    @Event global
    @Condition 1 + 2 == 4
    g = 1

rule "true with live":
    @Event global
    @Condition g > 1
    @Condition 1 == 1
    h = 1

rule "live then true then live":
    @Event global
    @Condition g > 1
    @Condition 1 == 1
    @Condition h > 1
    h = 2

rule "false with live":
    @Event global
    @Condition g > 1
    @Condition 1 == 2
    g = 3

rule "several true":
    @Event global
    @Condition 1 == 1
    @Condition 2 == 2
    g = 4

rule "live":
    @Event global
    @Condition g == 3
    g = 5

rule "and with true":
    @Event global
    @Condition g > 1 and 1 == 1
    g = 6

rule "or with true":
    @Event global
    @Condition g > 1 or 1 == 1
    g = 7

rule "negated false":
    @Event global
    @Condition not (1 == 2)
    g = 8

rule "disabled true":
    @Event global
    @Disabled
    @Condition 1 == 1
    g = 9

rule "delimiter true":
    @Event global
    @Condition 1 == 1
    @Delimiter
    g = 10

rule "delimiter false":
    @Event global
    @Condition 1 == 2
    @Delimiter
    g = 11
