#!optimizeForSize
#!optimizeStrict

globalvar g

rule "before include":
    g = 1 + 2

#!include "child.opy"

rule "after include":
    g = 2 * 3
