globalvar a
globalvar b
globalvar c
globalvar d
globalvar e
globalvar f
globalvar g

rule "default":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, c, d))
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, a, b))
    bigMessage(eventPlayer, "{3}{0}{2}{1}".format(a, b, c, d))
    bigMessage(eventPlayer, "{0}{3}{0}{2}{4}{1}{0}".format(a, b, c, d, e))
    bigMessage(eventPlayer, "{} {} {} {} {} {} {}".format(a, b, c, d, e, f, g))
    bigMessage(eventPlayer, "{0}{1}{2}{3}".format(a, b, c, d, e))
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, c, "{} {} {} {}".format(a, b, c, d)))

#!disableOptimizations
rule "disabled":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, c, d))
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, a, b))
    bigMessage(eventPlayer, "{3}{0}{2}{1}".format(a, b, c, d))
    bigMessage(eventPlayer, "{0}{3}{0}{2}{4}{1}{0}".format(a, b, c, d, e))
    bigMessage(eventPlayer, "{} {} {} {} {} {} {}".format(a, b, c, d, e, f, g))
    bigMessage(eventPlayer, "{0}{1}{2}{3}".format(a, b, c, d, e))
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, c, "{} {} {} {}".format(a, b, c, d)))

#!enableOptimizations

#!optimizeStrict
rule "strict":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, c, d))
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, a, b))
    bigMessage(eventPlayer, "{3}{0}{2}{1}".format(a, b, c, d))
    bigMessage(eventPlayer, "{0}{3}{0}{2}{4}{1}{0}".format(a, b, c, d, e))
    bigMessage(eventPlayer, "{} {} {} {} {} {} {}".format(a, b, c, d, e, f, g))
    bigMessage(eventPlayer, "{0}{1}{2}{3}".format(a, b, c, d, e))
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, c, "{} {} {} {}".format(a, b, c, d)))

#!disableOptimizeStrict

#!optimizeForSize
rule "size":
    @Event eachPlayer
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, c, d))
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, a, b))
    bigMessage(eventPlayer, "{3}{0}{2}{1}".format(a, b, c, d))
    bigMessage(eventPlayer, "{0}{3}{0}{2}{4}{1}{0}".format(a, b, c, d, e))
    bigMessage(eventPlayer, "{} {} {} {} {} {} {}".format(a, b, c, d, e, f, g))
    bigMessage(eventPlayer, "{0}{1}{2}{3}".format(a, b, c, d, e))
    bigMessage(eventPlayer, "{} {} {} {}".format(a, b, c, "{} {} {} {}".format(a, b, c, d)))
