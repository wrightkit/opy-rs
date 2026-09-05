#!disableOptimizations
rule "before include":
    pass

#!include "child.opy"

#!enableOptimizations
rule "after include":
    pass
