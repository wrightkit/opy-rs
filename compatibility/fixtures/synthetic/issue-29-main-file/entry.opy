globalvar value
#!include "child.opy"

rule "main-file entry":
    @Event global
    value = 1
