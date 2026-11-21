#!optimizeForSize

globalvar g 0

rule "skip counts":
    @Event global
    __skipIf__(numberOfActions=3, condition=g == 1)
    g = 5
    g = 6
    g = 7
    __skipIf__(g == 2, 0)
    __skipIf__(g == 3, 1)
    g = 8

#!disableOptimizations

rule "skip zero without optimization":
    @Event global
    __skipIf__(g == 4, 0)
    g = 9
