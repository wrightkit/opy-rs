#!allowMacroRedeclaration
#!translations EN zh-cn EN
#!rulePrefixTemplate
#!rulePrefix "Outer"
#!define VALUE 1
#!define VALUE 2

enum Kind:
    First
    First

macro helper():
    pass

macro helper():
    pass

rule "before":
    pass

#!include "child.opy"

def worker():
    @Name "Friendly worker"
    pass

rule "after":
    pass
