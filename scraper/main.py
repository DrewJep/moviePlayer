# import beautifulsoup4
import os
import requests

OMDB_APIKEY = os.environ.get('OMDB_APIKEY')


def confirm_existance(title):
    if not OMDB_APIKEY:
        raise RuntimeError('OMDB_APIKEY not set in environment')
    params = {'t': title, 'apikey': OMDB_APIKEY}
    response = requests.get('http://www.omdbapi.com/', params=params)
    data = response.json()
    return data.get('Title'), data.get('Year'), data.get('imdbID')

def fetch_page(url):
    response = requests.get(url)
    response.raise_for_status()  # Ensure we notice bad responses
    return response.text
def parse_html(html):   
    soup = beautifulsoup4.BeautifulSoup(html, 'html.parser')
    titles = [tag.get_text() for tag in soup.find_all('h1')]
    return titles
def main():
    # url = 'https://example.com'
    # html = fetch_page(url)
    # titles = parse_html(html)
    # for title in titles:
    #     print(title)
    print (confirm_existance("Inception"))

if __name__ == "__main__":
    main()