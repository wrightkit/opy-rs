#!replace0ByCapturePercentage
globalvar arr
globalvar result

rule "capture directive without size optimization":
    @Event global
    result = arr.index(0)

#!optimizeForSize
rule "literal slots precede capture replacement":
    @Event global
    result = arr.index(0)
    result = arr.index(1)
    result = 0 in arr
    result = arr.index(arr.index(0))
    wait(0)
    print(0)
