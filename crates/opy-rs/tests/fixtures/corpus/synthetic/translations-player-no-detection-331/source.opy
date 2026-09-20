#!translations en fr
#!translateWithPlayerVar noDetectionRule

rule "translation player no detection":
    @Event eachPlayer
    bigMessage(eventPlayer, _("Hello"))
