#!disableOptimizations
globalvar g
globalvar h

rule "unoptimized":
    @Event global
    g = -5
    g = -h
    g = -(-h)
    g = 2 + 3
    g = h * 0
