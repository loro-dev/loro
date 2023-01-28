from pyloro import Loro, LoroText;

loro = Loro()
text = loro.get_text("text")
text.insert(loro, 0, "123")
print(text.value())