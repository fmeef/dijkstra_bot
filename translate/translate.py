import requests
from googletrans import Translator
import argparse
import yaml
from yaml import CLoader as Loader
from typing import List
import re
import os

FORM = re.compile(r'{}')
MAGIC_WORD = re.compile(r'@@@*') # eldritch phrase that can't be translated
class ValueToHashtag:
    """
    Google translate apparently *sometimes* leave hashtags untranslated.
    we can use them as identifiers for formatting
    """
    def __init__(self, lang: str):
        self.lang = lang
        self.t = Translator()
        self.lines: List[str] = []
    
    def value_to_hashtag(self, value: str) -> str:
        value = MAGIC_WORD.sub('{}', value)
        trans = self.t.translate(value, self.lang)
        self.lines.append(trans.text)
        return trans.text

    def get_lines(self):
        for line in self.lines:
            yield MAGIC_WORD.sub('{}', line)           
            
def translate(source: str, langs: List[str]):
    with open(source, 'r') as f:
        contents = yaml.load(f, Loader)
        for lang in langs:
            out_yaml = {}
            translated_keys = []

            out_path = os.path.dirname(source)
            out_path = os.path.join(out_path, f'{lang}.yaml')
            
            if os.path.exists(out_path):
                with open(out_path, 'r') as n:
                    out_yaml = yaml.load(n, Loader)
            hashtag = ValueToHashtag(lang)
            for key, value in contents.items():
                if key in out_yaml:
                    continue
                out = hashtag.value_to_hashtag(value)
                translated_keys.append(key)
                print(out)
            print(f'{lang} complete')

            for key, line in zip(translated_keys ,hashtag.get_lines()):
                #print(type(line))
                out_yaml[key] = line
            with open(out_path, 'w') as f:
                yaml.dump(out_yaml, f, allow_unicode=True, encoding='utf-8')


def run():
    parser = argparse.ArgumentParser(
        prog='translate',
        description='Translation script for dijkstra bot strings'
    )

    parser.add_argument('source', type=str, help='Yaml resource file containing strings to translate')
    parser.add_argument('langs', type=str, nargs='+', help='List of languages to translate to')
    args = parser.parse_args()

    return translate(args.source, args.langs)

run()