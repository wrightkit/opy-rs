globalvar t 0
globalvar s 1

rule "repro":
    s = (t % 60 + 100).substring(1, 2)
