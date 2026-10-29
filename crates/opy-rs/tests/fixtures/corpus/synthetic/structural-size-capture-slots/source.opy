#!replace0ByCapturePercentage
globalvar arr
globalvar result

rule "capture directive without size optimization":
    @Event global
    result = arr.index(0)
    result = max(result, 0)
    result = min(result, 0)
    result = (result == 0)
    result = arr.index(result == 0)

#!optimizeForSize
rule "literal slots precede capture replacement":
    @Event global
    result = arr.index(0)
    result = arr.index(1)
    result = 0 in arr
    result = arr.index(arr.index(0))
    wait(0)
    print(0)
    result = max(result, 0)
    result = min(result, 0)
    result = max(arr, 0)
    result = min(arr, 0)
    result = (result == 0)
    result = arr.index(result == 0)

rule "capture comparison condition":
    @Event global
    @Condition result == 0
    result = 1

#!disableOptimizations
rule "capture directive with optimizations disabled":
    @Event global
    result = max(result, 0)
    result = min(result, 0)
    result = (result == 0)
    result = arr.index(result == 0)
