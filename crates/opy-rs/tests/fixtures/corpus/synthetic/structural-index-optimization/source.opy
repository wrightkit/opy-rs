globalvar arr
globalvar result

rule "enabled zero indexing":
    @Event global
    result = arr[0]
    result = arr[0.0]
    result = arr[1]

#!disableOptimizations

rule "disabled zero indexing":
    @Event global
    result = arr[0]
    result = arr[0.0]
    result = arr[1]
    result = [][0]
    result = [][0.0]
    result = [][1]

#!enableOptimizations

rule "reenabled zero indexing":
    @Event global
    result = arr[0]

#!optimizeStrict

rule "strict empty array indexing":
    @Event global
    result = [][0]
    result = [][0.0]
    result = [][1]
    result = [][0.1]
    result = [][0.49]
