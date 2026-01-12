# import beautifulsoup4
import requests

def confirm_existance(title):
    response = requests.get(f'http://www.omdbapi.com/?t={title}&apikey=f1f36f17')
    data =  response.json()
    return data['Title'], data['Year'], data['imdbID']

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