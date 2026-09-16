globalvar A
globalvar B

rule "non-strict before optimizer":
    @Event global
    A = "am" == "**"

#!optimizeStrict
rule "strict optimizer semantics":
    @Event global
    A = B + 0
    A = B * 0
    A = B * 1
    A = B / 1
    A = B or true
    A = B and false
    A = "am" == "**"

#!disableOptimizeStrict
rule "non-strict after optimizer":
    @Event global
    A = "am" == "**"
