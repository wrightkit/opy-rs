#!useVariableForCompressionAlphabet
#!optimizeStrict
globalvar a
globalvar b
globalvar c

rule "compression":
    @Event global
    a = compressed([1, 2, 3])
    splitDictArray({first: b, second: c}, [{first: 1, second: null}, {first: 22, second: 3}], true)
