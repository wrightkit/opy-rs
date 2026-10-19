#!disableOptimizations

rule "child rule":
    g = 3 - 1

#!enableOptimizations
#!disableOptimizeStrict

rule "child rule 2":
    g = 4 + 5
