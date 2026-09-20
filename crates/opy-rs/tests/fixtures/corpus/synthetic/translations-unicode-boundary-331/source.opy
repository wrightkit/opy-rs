#!translations fr

rule "translation unicode boundary":
    @Event eachPlayer
    bigMessage(eventPlayer, _("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA界界界界界界界界界界{}".format(eventPlayer)))
