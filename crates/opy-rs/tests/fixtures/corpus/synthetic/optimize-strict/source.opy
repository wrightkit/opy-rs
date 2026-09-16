globalvar A
globalvar B

#!optimizeStrict
rule "strict optimizer semantics":
    @Event global
    A = B + 0
    A = B * 0
    A = B * 1
    A = B / 1
    A = B or true
    A = B and false
